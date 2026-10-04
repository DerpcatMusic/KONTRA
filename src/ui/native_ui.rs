//! Render the library's own NativeUI graph. Lua belongs to the editor.
use super::{Cx, theme::*};
use crate::native_ui::{Package, Session};
use mlua::{Function, Table, Value};
use moose::mui::mui::prelude::*;
use moose::mui::mui::{geometry::Path as DrawPath, scene::Fit};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

pub(super) struct State {
    epoch: u64,
    controls: Arc<[crate::ksp::ExposedControl]>,
    package: Arc<Package>,
    session: Option<Session>,
    graph: Option<Table>,
    hovered: HashSet<String>,
    error: String,
    revision: u64,
    text_drafts: HashMap<String, String>,
}
impl State {
    fn new(
        epoch: u64,
        package: Arc<Package>,
        entry: &str,
        controls: Arc<[crate::ksp::ExposedControl]>,
    ) -> Self {
        let (session, error) = match Session::new(package.clone(), entry, controls.clone()) {
            Ok(session) => (Some(session), String::new()),
            Err(e) => (None, format!("NativeUI could not start: {e:#}")),
        };
        Self {
            epoch,
            controls,
            package,
            session,
            graph: None,
            hovered: HashSet::new(),
            error,
            revision: 0,
            text_drafts: HashMap::new(),
        }
    }
}
pub(super) fn revision(cx: &Cx, slot: usize) -> u64 {
    cx.state
        .native_ui
        .get(&slot)
        .map_or(0, |state| state.revision)
}
pub(super) fn view(ui: &mut Ui, cx: &mut Cx, slot: usize, room: Size) -> El {
    let part = &cx.view.parts[slot];
    let entry = part.interface.as_ref().map_or("", |i| i.native_ui.as_str());
    let Some(package) = part.native_ui.clone() else {
        return caption("NativeUI resources could not be loaded. See Logs for details.")
            .lines(3)
            .pad(12);
    };
    let state = cx.state.native_ui.entry(slot).or_insert_with(|| {
        State::new(
            part.script_epoch,
            package.clone(),
            entry,
            part.exposed_controls.clone(),
        )
    });
    if state.epoch != part.script_epoch || !Arc::ptr_eq(&state.package, &package) {
        *state = State::new(
            part.script_epoch,
            package,
            entry,
            part.exposed_controls.clone(),
        );
    }
    let Some(session) = &state.session else {
        return caption(state.error.clone()).lines(5).pad(12);
    };
    cx.state
        .meters
        .animating
        .store(true, std::sync::atomic::Ordering::Relaxed);
    if !Arc::ptr_eq(&state.controls, &part.exposed_controls) {
        state.controls = part.exposed_controls.clone();
        session.update(state.controls.clone());
    }
    let authored = state
        .graph
        .as_ref()
        .map(authored_size)
        .unwrap_or(Size::new(970., 600.));
    let scale = super::perf_view::scale_to_fit(room, authored, cx.settings.view_scale);
    let result = (|| -> anyhow::Result<El> {
        if let Some(graph) = &state.graph {
            events(ui, graph, session, slot, scale, &mut state.hovered)?;
        }
        let graph = session.render()?;
        let authored = authored_size(&graph);
        let scale = super::perf_view::scale_to_fit(room, authored, cx.settings.view_scale);
        let el = draw(
            ui,
            &graph,
            &state.package,
            session,
            slot,
            scale,
            Style::default(),
            &mut state.text_drafts,
        )?
        .w(authored.width * scale)
        .h(authored.height * scale)
        .named("NativeUI performance view");
        if session
            .lua()
            .globals()
            .get::<bool>("__resign_focus")
            .unwrap_or(false)
        {
            ui.blur();
            session.lua().globals().set("__resign_focus", false)?;
        }
        state.graph = Some(graph);
        for edit in session.take_edits() {
            let queued = if let Some(active) = edit.midi_learn {
                cx.p.shared.learn_native_control(
                    slot,
                    state.epoch,
                    edit.slot,
                    edit.control,
                    edit.index,
                    active,
                )
            } else {
                match &edit.text {
                    Some(text) => cx.p.shared.edit_native_text(
                        slot,
                        state.epoch,
                        edit.slot,
                        edit.control,
                        edit.index,
                        text,
                    ),
                    None => cx.p.shared.edit_native_control(
                        slot,
                        state.epoch,
                        edit.slot,
                        edit.control,
                        edit.index,
                        edit.value,
                    ),
                }
            };
            anyhow::ensure!(queued, "NativeUI control edit could not be queued");
        }
        let unavailable = session.unavailable_controls();
        let view = row![el].justify(Justify::Center).w(Len::Pct(100.));
        Ok(if unavailable.is_empty() {
            view
        } else {
            col![
                caption(format!(
                    "NativeUI references unavailable KSP controls: {}",
                    unavailable.join(", ")
                ))
                .lines(2)
                .pad(4),
                view
            ]
        })
    })();
    state.revision = state.revision.wrapping_add(1);
    match result {
        Ok(el) => el,
        Err(e) => {
            state.error = format!("NativeUI: {e:#}");
            caption(state.error.clone()).lines(6).pad(12)
        }
    }
}
fn tables(t: &Table, key: &str) -> mlua::Result<Vec<Table>> {
    t.get::<Table>(key)?.sequence_values::<Table>().collect()
}
fn number(t: &Table, key: &str) -> Option<f64> {
    match t.get::<Value>(key).ok()? {
        Value::Integer(n) => Some(n as f64),
        Value::Number(n) => Some(n),
        _ => None,
    }
}
fn string(t: &Table, key: &str) -> String {
    t.get::<String>(key).unwrap_or_default()
}
fn boolean(v: &Value) -> bool {
    !matches!(v, Value::Nil | Value::Boolean(false))
}
fn color(v: &Value) -> Option<Color> {
    let Value::Table(t) = v else { return None };
    Some(
        Color::srgb(
            number(t, "r")? as f32,
            number(t, "g")? as f32,
            number(t, "b")? as f32,
        )
        .with_alpha(number(t, "a").unwrap_or(1.) as f32),
    )
}
fn val(t: &Table, key: &str) -> Value {
    t.get(key).unwrap_or(Value::Nil)
}
fn authored_size(graph: &Table) -> Size {
    let mut size = Size::new(970., 600.);
    if let Ok(modifiers) = tables(graph, "modifiers") {
        for m in modifiers {
            if string(&m, "name") == "frame"
                && let Ok(t) = m.get::<Table>("value")
            {
                if let Some(w) = number(&t, "width") {
                    size.width = w;
                }
                if let Some(h) = number(&t, "height") {
                    size.height = h;
                }
            }
        }
    }
    size
}
#[derive(Clone)]
struct Style {
    ink: Color,
    size: f64,
    bold: bool,
    disabled: bool,
    hidden: bool,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            ink: Color::srgb(1., 1., 1.),
            size: 12.,
            bold: false,
            disabled: false,
            hidden: false,
        }
    }
}
fn align(value: &str) -> (Align, Align) {
    (
        if value.contains("left") {
            Align::Start
        } else if value.contains("right") {
            Align::End
        } else {
            Align::Center
        },
        if value.contains("top") {
            Align::Start
        } else if value.contains("bottom") {
            Align::End
        } else {
            Align::Center
        },
    )
}
// Flexible primitives consume the proposed frame; text and fixed frames hug
// their intrinsic size. Keep this across modifier wrappers rather than making
// every component expand when an outer frame is applied.
fn flexibility(node: &Table) -> (bool, bool) {
    let kind = string(node, "kind");
    let children = tables(node, "children").unwrap_or_default();
    let mut flex = match kind.as_str() {
        "ZStack" | "Group" | "HStack" | "VStack" => {
            let mut f = (false, false);
            for child in children {
                let c = flexibility(&child);
                if string(&child, "kind") == "Spacer" {
                    let frames = tables(&child, "modifiers").unwrap_or_default();
                    let fixed = |axis| {
                        frames.iter().any(|m| {
                            string(m, "name") == "frame"
                                && m.get::<Table>("value")
                                    .ok()
                                    .is_some_and(|t| number(&t, axis).is_some())
                        })
                    };
                    f.0 |= kind == "HStack" && !fixed("width");
                    f.1 |= kind == "VStack" && !fixed("height");
                }
                f.0 |= c.0;
                f.1 |= c.1;
                if kind == "ZStack" {
                    for m in tables(&child, "modifiers").unwrap_or_default() {
                        if string(&m, "name") == "align" {
                            let a = string(&m, "value");
                            f.0 |= a.contains("left") || a.contains("right");
                            f.1 |= a.contains("top") || a.contains("bottom");
                        }
                    }
                }
            }
            f
        }
        _ => (false, false),
    };
    for m in tables(node, "modifiers").unwrap_or_default() {
        if string(&m, "name") == "frame" {
            if let Ok(t) = m.get::<Table>("value") {
                if number(&t, "width").is_some() {
                    flex.0 = false;
                } else if number(&t, "max_width").is_some_and(f64::is_finite) {
                    flex.0 = false;
                } else if number(&t, "max_width").is_some_and(|n| n.is_infinite()) {
                    flex.0 = true;
                }
                if number(&t, "height").is_some() {
                    flex.1 = false;
                } else if number(&t, "max_height").is_some_and(f64::is_finite) {
                    flex.1 = false;
                } else if number(&t, "max_height").is_some_and(|n| n.is_infinite()) {
                    flex.1 = true;
                }
            }
        }
    }
    flex
}
fn expand(mut el: El, flex: (bool, bool)) -> El {
    if flex.0 {
        el = el.w(Len::Pct(100.));
    }
    if flex.1 {
        el = el.h(Len::Pct(100.));
    }
    el
}
fn draw(
    ui: &mut Ui,
    node: &Table,
    package: &Arc<Package>,
    session: &Session,
    slot: usize,
    s: f64,
    mut style: Style,
    drafts: &mut HashMap<String, String>,
) -> anyhow::Result<El> {
    let props: Table = node.get("props")?;
    let modifiers = tables(node, "modifiers")?;
    let kind = string(node, "kind");
    for m in &modifiers {
        let v = val(m, "value");
        match string(m, "name").as_str() {
            "foreground_color" => {
                if let Some(c) = color(&v) {
                    style.ink = c;
                }
            }
            "font_size" => {
                if let Some(n) = number(m, "value") {
                    style.size = n;
                }
            }
            "bold" => style.bold = boolean(&v),
            "disabled" => style.disabled |= boolean(&v),
            "hidden" => style.hidden |= boolean(&v),
            _ => {}
        }
    }
    let child_nodes = tables(node, "children")?;
    let has_flexible_content = child_nodes.iter().any(|child| {
        let flex = flexibility(child);
        string(child, "kind") != "Spacer"
            && ((kind == "HStack" && flex.0) || (kind == "VStack" && flex.1))
    });
    let mut children = child_nodes
        .iter()
        .map(|child| {
            let mut el = draw(ui, child, package, session, slot, s, style.clone(), drafts)?;
            let flexible = flexibility(child);
            if has_flexible_content && string(child, "kind") == "Spacer" {
                // Explicit flexible content consumes the proposal first;
                // spacers keep their minimum instead of stealing half of it.
                el = el.grow(0.);
            }
            // A flexible sibling takes the remaining stack space, after fixed
            // siblings and gaps. 100% of the entire stack would overflow it.
            if kind == "HStack" && flexible.0 {
                el = el.w(Len::Auto).grow(1.);
            } else if kind == "VStack" && flexible.1 {
                el = el.h(Len::Auto).grow(1.);
            }
            // MUI's default stack alignment couples the axes for Start.
            // NativeUI specifies them independently, e.g. top center.
            let explicit = tables(child, "modifiers")?
                .iter()
                .any(|m| string(m, "name") == "align");
            Ok(if kind == "ZStack" && !explicit {
                let (x, y) = align(&string(&props, "alignment"));
                el.anchor(x, y)
            } else {
                el
            })
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let spacing = number(&props, "spacing").unwrap_or(0.) * s;
    if spacing < 0. {
        children = children
            .into_iter()
            .enumerate()
            .map(|(i, child)| {
                if kind == "HStack" {
                    child.offset(i as f64 * spacing, 0.)
                } else {
                    child.offset(0., i as f64 * spacing)
                }
            })
            .collect();
    }
    let mut el = match kind.as_str() {
        "HStack" => row(children)
            .gap(spacing.max(0.))
            .align(align(&string(&props, "alignment")).1),
        "VStack" => col(children)
            .gap(spacing.max(0.))
            .align(align(&string(&props, "alignment")).0),
        "ZStack" | "Group" => {
            let (x, y) = align(&string(&props, "alignment"));
            stack(children).align(x).justify(match y {
                Align::Start => Justify::Start,
                Align::End => Justify::End,
                _ => Justify::Center,
            })
        }
        "Spacer" => block(0, 0)
            .min_w(number(&props, "min_length").unwrap_or(0.) * s)
            .grow(1.),
        "Text" => text(string(&props, "text"))
            .text_size(style.size * s)
            .fill(style.ink)
            .text_weight(if style.bold {
                mui_text::Weight::BOLD
            } else {
                mui_text::Weight::REGULAR
            }),
        "TextInput" => {
            let id = format!("nui-{slot}-{}-text", string(node, "path"));
            let value = drafts
                .entry(id.clone())
                .or_insert_with(|| string(&props, "text"));
            if !ui.focused(id.as_str()) {
                *value = string(&props, "text");
            }
            let field = text_edit(ui, id, value, TextOpts::default());
            if field.changed.changed
                && let Ok(f) = props.get::<Function>("on_change")
            {
                session.call(f, value.clone())?;
            }
            if field.changed.submitted
                && let Ok(f) = props.get::<Function>("on_accept")
            {
                session.call(f, value.clone())?;
            }
            field
                .el
                .text_size(style.size * s)
                .fill(Fill::None)
                .pad(0)
                .radius(0.)
        }
        "Rectangle" => block(Len::Auto, Len::Auto)
            .fill(color(&val(&props, "color")).unwrap_or(style.ink))
            .radius(number(&props, "corner_radius").unwrap_or(0.) * s),
        "Image" => {
            let file = string(&props, "file").replace('\\', "/").to_lowercase();
            let image = package
                .images
                .get(&file)
                .ok_or_else(|| anyhow::anyhow!("Missing NativeUI image {file:?}"))?
                .clone();
            let resizable = boolean(&val(&props, "resizable"));
            let el = block(
                if resizable {
                    Len::Auto
                } else {
                    Len::Px(f64::from(image.width) * s)
                },
                if resizable {
                    Len::Auto
                } else {
                    Len::Px(f64::from(image.height) * s)
                },
            )
            .fill(Fill::Image(image, Fit::Fill));
            if let Some(ink) = color(&val(&props, "color")) {
                el.mask(ink)
            } else {
                el
            }
        }
        "Ring" => {
            let from = number(&props, "start_angle").unwrap_or(0.).to_radians();
            let angle = number(&props, "angle").unwrap_or(360.).to_radians();
            let thickness = number(&props, "thickness").unwrap_or(1.) * s;
            let ink = color(&val(&props, "color")).unwrap_or(style.ink);
            canvas(move |size| {
                let radius = (size.width.min(size.height) - thickness).max(0.) / 2.;
                let count = (angle.abs() * radius).ceil().clamp(2., 2048.) as usize;
                let points = (0..=count).map(|i| {
                    let a = from + angle * i as f64 / count as f64;
                    Point::new(
                        size.width / 2. + radius * a.cos(),
                        size.height / 2. + radius * a.sin(),
                    )
                });
                vec![Draw::stroke(
                    DrawPath::polyline(points, false),
                    ink,
                    thickness,
                )]
            })
        }
        "Canvas" => {
            let paint: Function = props.get("paint")?;
            let vm = session.lua().clone();
            canvas(move |size| {
                match canvas_draw(&vm, &paint, Size::new(size.width / s, size.height / s), s) {
                    Ok(draw) => draw,
                    Err(e) => {
                        let _ = vm.globals().set("__canvas_error", e.to_string());
                        eprintln!("NativeUI canvas: {e}");
                        Vec::new()
                    }
                }
            })
        }
        other => anyhow::bail!("Unsupported NativeUI primitive {other:?}"),
    }
    .shrink(0);
    let base = session.lua().create_table()?;
    base.set("kind", kind.clone())?;
    base.set("props", props.clone())?;
    base.set("children", node.get::<Table>("children")?)?;
    base.set("modifiers", session.lua().create_table()?)?;
    let mut flex = flexibility(&base);
    if matches!(kind.as_str(), "Canvas" | "Rectangle")
        || (kind == "Image" && boolean(&val(&props, "resizable")))
    {
        flex = (true, true);
    }
    el = expand(el, flex);
    let container = matches!(kind.as_str(), "HStack" | "VStack" | "ZStack");
    let mut fixed = (false, false);
    for m in modifiers {
        let name = string(&m, "name");
        let value = val(&m, "value");
        match name.as_str() {
            "frame" => {
                if let Value::Table(t) = value {
                    let (x, y) = align(&string(&t, "alignment"));
                    if container {
                        if !fixed.0 && number(&t, "width").is_some() {
                            el = el.w(Len::Pct(100.));
                        }
                        if !fixed.1 && number(&t, "height").is_some() {
                            el = el.h(Len::Pct(100.));
                        }
                    }
                    fixed.0 |= number(&t, "width").is_some();
                    fixed.1 |= number(&t, "height").is_some();
                    if matches!(kind.as_str(), "Ring" | "Canvas" | "Rectangle" | "Image") {
                        if let Some(w) = number(&t, "width") {
                            el = el.w(w.max(0.) * s);
                        }
                        if let Some(h) = number(&t, "height") {
                            el = el.h(h.max(0.) * s);
                        }
                    }
                    let mut frame = stack([el.anchor(x, y)]).align(x).justify(match y {
                        Align::Start => Justify::Start,
                        Align::End => Justify::End,
                        _ => Justify::Center,
                    });
                    if let Some(w) = number(&t, "width") {
                        frame = frame.w(w.max(0.) * s)
                    } else if number(&t, "max_width").is_some_and(|n| n.is_infinite()) {
                        frame = frame.w(Len::Pct(100.)).grow(1.)
                    }
                    if let Some(h) = number(&t, "height") {
                        frame = frame.h(h.max(0.) * s)
                    } else if number(&t, "max_height").is_some_and(|n| n.is_infinite()) {
                        frame = frame.h(Len::Pct(100.)).grow(1.)
                    }
                    let maximum = |key| {
                        number(&t, key)
                            .filter(|n| n.is_finite())
                            .map_or(1e6, |n| (n.max(0.) * s).min(1e6))
                    };
                    if number(&t, "max_width").is_some_and(f64::is_finite)
                        || number(&t, "max_height").is_some_and(f64::is_finite)
                    {
                        frame =
                            frame.max_size(Size::new(maximum("max_width"), maximum("max_height")));
                    }
                    if let Some(w) = number(&t, "max_width").filter(|n| n.is_finite()) {
                        frame = frame.w(w.max(0.) * s);
                        flex.0 = false;
                    }
                    if let Some(h) = number(&t, "max_height").filter(|n| n.is_finite()) {
                        frame = frame.h(h.max(0.) * s);
                        flex.1 = false;
                    }
                    if let Some(w) = number(&t, "min_width") {
                        frame = frame.min_w(w.max(0.) * s)
                    }
                    if let Some(h) = number(&t, "min_height") {
                        frame = frame.min_h(h.max(0.) * s)
                    }
                    if number(&t, "width").is_some() {
                        flex.0 = false;
                    } else if number(&t, "max_width").is_some_and(|n| n.is_infinite()) {
                        flex.0 = true;
                    }
                    if number(&t, "height").is_some() {
                        flex.1 = false;
                    } else if number(&t, "max_height").is_some_and(|n| n.is_infinite()) {
                        flex.1 = true;
                    }
                    el = expand(frame, flex);
                }
            }
            "position" => {
                if let Value::Table(t) = value {
                    el = el.at(
                        number(&t, "x").unwrap_or(0.) * s,
                        number(&t, "y").unwrap_or(0.) * s,
                    )
                }
            }
            "offset" => {
                if let Value::Table(t) = value {
                    el = el.offset(
                        number(&t, "x").unwrap_or(0.) * s,
                        number(&t, "y").unwrap_or(0.) * s,
                    )
                }
            }
            "align" => {
                if let Value::String(v) = value {
                    let (x, y) = align(&v.to_string_lossy());
                    el = el.anchor(x, y)
                }
            }
            "padding" => {
                let (top, right, bottom, left) = if let Value::Table(t) = value {
                    let a = number(&t, "all").unwrap_or(0.);
                    let h = number(&t, "horizontal").unwrap_or(a);
                    let v = number(&t, "vertical").unwrap_or(a);
                    (
                        number(&t, "top").unwrap_or(v),
                        number(&t, "right").unwrap_or(h),
                        number(&t, "bottom").unwrap_or(v),
                        number(&t, "left").unwrap_or(h),
                    )
                } else {
                    let n = number(&m, "value").unwrap_or(0.);
                    (n, n, n, n)
                };
                el = expand(
                    stack([el.offset(left.min(0.) * s, top.min(0.) * s)]).pad(edges(
                        top.max(0.) * s,
                        right.max(0.) * s,
                        bottom.max(0.) * s,
                        left.max(0.) * s,
                    )),
                    flex,
                );
            }
            "background" | "overlay" => {
                if let Value::Table(t) = value {
                    let (x, y) = align(&string(&m, "alignment"));
                    let mut background =
                        draw(ui, &t, package, session, slot, s, style.clone(), drafts)?;
                    if !string(&m, "alignment").is_empty() {
                        background = background.anchor(x, y);
                    }
                    background = if name == "background" {
                        background.underlay()
                    } else {
                        background.overlay()
                    };
                    if string(&t, "kind") == "Rectangle" {
                        background = background.full();
                    }
                    el = expand(
                        if name == "background" {
                            stack([background, el])
                        } else {
                            stack([el, background])
                        },
                        flex,
                    );
                }
            }
            "hidden" => {
                if boolean(&value) {
                    el = el.opacity(0.).disabled()
                }
            }
            "opacity" => el = el.opacity(number(&m, "value").unwrap_or(1.) as f32),
            "line_limit" => el = el.lines(number(&m, "value").unwrap_or(1.).max(1.) as usize),
            "popover" => {
                if let Value::Table(t) = value {
                    let content: Table = t.get("content")?;
                    let popup_id = format!("nui-{slot}-{}-popup", string(node, "path"));
                    let parent_id = format!("nui-{slot}-{}", string(node, "path"));
                    let parent = ui
                        .scene()
                        .and_then(|scene| scene.surface(&parent_id))
                        .map_or(Size::ZERO, |surface| surface.frame.size);
                    let size = ui
                        .scene()
                        .and_then(|scene| scene.surface(&popup_id))
                        .map_or(Size::ZERO, |surface| surface.frame.size);
                    let spacing = number(&t, "spacing").unwrap_or(0.) * s;
                    let (xa, ya) = align(&string(&t, "alignment"));
                    let x = match xa {
                        Align::Start => 0.,
                        Align::End => parent.width - size.width,
                        _ => (parent.width - size.width) / 2.,
                    };
                    let y = match ya {
                        Align::Start => 0.,
                        Align::End => parent.height - size.height,
                        _ => (parent.height - size.height) / 2.,
                    };
                    let (x, y) = match string(&t, "direction").as_str() {
                        "up" => (x, -size.height - spacing),
                        "left" => (-size.width - spacing, y),
                        "right" => (parent.width + spacing, y),
                        _ => (x, parent.height + spacing),
                    };
                    let popup = draw(
                        ui,
                        &content,
                        package,
                        session,
                        slot,
                        s,
                        style.clone(),
                        drafts,
                    )?
                    .id(popup_id)
                    .tracks_pointer()
                    .float()
                    .at(x, y);
                    el = stack([el, popup]);
                }
            }
            "rotation" => el = stack([el]).rotation(number(&m, "value").unwrap_or(0.).to_radians()),
            _ => {}
        }
    }
    for m in tables(node, "modifiers")? {
        if string(&m, "name") == "align" {
            let (x, y) = align(&string(&m, "value"));
            el = el.anchor(x, y);
        }
    }
    let path = string(node, "path");
    let interactive = tables(node, "modifiers")?
        .iter()
        .any(|m| string(m, "name").starts_with("on_") || string(m, "name") == "popover");
    if interactive {
        el = el
            .id(format!("nui-{slot}-{path}"))
            .tracks_pointer()
            .focusable();
    }
    if style.disabled {
        el = el.disabled();
    }
    if style.hidden {
        el = el.opacity(0.).disabled();
    }
    Ok(el)
}
fn events(
    ui: &mut Ui,
    node: &Table,
    session: &Session,
    slot: usize,
    s: f64,
    hovered: &mut HashSet<String>,
) -> anyhow::Result<()> {
    let path = string(node, "path");
    let id = format!("nui-{slot}-{path}");
    let r = ui.get(id.as_str());
    let was = hovered.contains(&path);
    if r.hovered {
        hovered.insert(path.clone());
    } else {
        hovered.remove(&path);
    }
    let local = ui.local(id.as_str()).unwrap_or(Point::ZERO);
    let frame = ui
        .scene()
        .and_then(|scene| scene.surface(&id))
        .map_or(Size::ZERO, |surface| surface.frame.size);
    let e = session.event(
        local.x / s,
        local.y / s,
        r.drag_delta.x / s,
        r.drag_delta.y / s,
        frame.width / s,
        frame.height / s,
        r.mods.shift,
        r.mods.ctrl,
        r.mods.alt,
        r.mods.cmd,
    )?;
    for m in tables(node, "modifiers")? {
        let name = string(&m, "name");
        let Value::Table(t) = val(&m, "value") else {
            continue;
        };
        let triggers: Vec<&str> = match name.as_str() {
            "on_tap_gesture" => [
                (r.pressed || r.key_activated).then_some("start"),
                r.double_clicked.then_some("double"),
                (r.released || r.key_activated).then_some("complete"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            "on_drag_gesture" => [
                r.pressed.then_some("start"),
                r.dragged.then_some("update"),
                r.released.then_some("complete"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            "on_hover_gesture" => [
                (!was && r.hovered).then_some("enter"),
                (was && !r.hovered).then_some("exit"),
                r.hovered.then_some("update"),
            ]
            .into_iter()
            .flatten()
            .collect(),
            _ => Vec::new(),
        };
        for trigger in triggers {
            if let Ok(f) = t.get::<Function>(trigger) {
                session.call(f, e.clone())?;
            }
        }
        if name == "background" || name == "overlay" {
            events(ui, &t, session, slot, s, hovered)?;
        }
        if name == "popover" {
            let popup_id = format!("{id}-popup");
            if string(&t, "auto_close_when") != "never"
                && ui.dismissed(&[id.as_str(), popup_id.as_str()])
            {
                if let Ok(close) = t.get::<Function>("on_auto_close") {
                    session.call(close, ())?;
                }
            } else if let Ok(content) = t.get::<Table>("content") {
                events(ui, &content, session, slot, s, hovered)?;
            }
        }
    }
    for child in tables(node, "children")? {
        events(ui, &child, session, slot, s, hovered)?;
    }
    Ok(())
}
fn canvas_draw(lua: &mlua::Lua, paint: &Function, size: Size, s: f64) -> mlua::Result<Vec<Draw>> {
    let frame = lua.create_table()?;
    frame.set("width", size.width)?;
    frame.set("height", size.height)?;
    let (commands, painter): (Table, Table) =
        lua.globals().get::<Function>("__painter")?.call(())?;
    paint.call::<()>((painter, frame))?;
    let mut out = Vec::new();
    for command in commands.sequence_values::<Table>() {
        let c = command?;
        let mut path = DrawPath::default();
        for step in c.get::<Table>("path")?.sequence_values::<Table>() {
            let t = step?;
            let n = |i| t.get::<f64>(i).unwrap_or(0.) * s;
            path = match t.get::<String>(1)?.as_str() {
                "move" => path.move_to(Point::new(n(2), n(3))),
                "line" => path.line_to(Point::new(n(2), n(3))),
                "cubic" => path.cubic_to(
                    Point::new(n(2), n(3)),
                    Point::new(n(4), n(5)),
                    Point::new(n(6), n(7)),
                ),
                "close" => path.close(),
                _ => path,
            };
        }
        if let Some(ink) = color(&val(&c, "fill")) {
            out.push(Draw::fill(path.clone(), ink));
        }
        if let Some(ink) = color(&val(&c, "stroke")) {
            let dash = match c.get::<Table>("dash") {
                Ok(t) => t
                    .sequence_values::<f64>()
                    .collect::<mlua::Result<Vec<_>>>()?,
                Err(_) => Vec::new(),
            };
            let width = number(&c, "width").unwrap_or(1.) * s;
            if dash.is_empty()
                || dash.len() > 64
                || dash.iter().any(|n| !n.is_finite() || *n * s < 0.25)
            {
                out.push(Draw::stroke(path, ink, width));
            } else {
                let mut dashed = DrawPath::default();
                for contour in path.flatten(0.25, 16384).map_err(mlua::Error::external)? {
                    let mut index = 0;
                    let mut left = dash[0] * s;
                    for pair in contour.windows(2) {
                        let (a, b) = (pair[0], pair[1]);
                        let dx = b.x - a.x;
                        let dy = b.y - a.y;
                        let length = dx.hypot(dy);
                        let mut at = 0.;
                        while at < length {
                            let step = left.min(length - at);
                            let point =
                                |n: f64| Point::new(a.x + dx * n / length, a.y + dy * n / length);
                            if index % 2 == 0 {
                                dashed = dashed.move_to(point(at)).line_to(point(at + step));
                            }
                            at += step;
                            left -= step;
                            if left <= f64::EPSILON {
                                index = (index + 1) % dash.len();
                                left = dash[index] * s;
                            }
                        }
                    }
                }
                out.push(Draw::stroke(dashed, ink, width));
            }
        }
    }
    Ok(out)
}
