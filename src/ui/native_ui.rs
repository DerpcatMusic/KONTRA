//! The authored legacy NativeUI graph, using the shared resource service.
use super::{
    native_runtime::{Edit, Package, Session},
    theme::*,
};
use mlua::{Function, Table, Value};
use moose::mui::mui::{geometry::Path as DrawPath, prelude::*, scene::Fit};
use sampler_ui_ir as ir;
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

// The editor builder is Send; its Lua VM is created and kept on the GUI thread.
// Weak lifetime tokens let any subsequent GUI paint retire a dropped Face.
struct Local {
    lifetime: std::sync::Weak<AtomicBool>,
    session: Session,
    graph: Option<Table>,
    hovered: HashSet<String>,
    drafts: HashMap<String, String>,
}
thread_local! {static LOCAL:std::cell::RefCell<HashMap<u64,Local>>=std::cell::RefCell::new(HashMap::new());}
static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
pub(super) struct State {
    id: u64,
    loading: std::sync::mpsc::Receiver<anyhow::Result<Arc<Package>>>,
    canceled: Arc<AtomicBool>,
    package: Option<Arc<Package>>,
    failed: bool,
    failure: Option<String>,
    started: bool,
    entry: String,
    seed: Vec<(ir::Source, usize, ir::Widget)>,
    size: Size,
}
fn failure(error: &anyhow::Error, phase: &str) -> String {
    fn category(error: &mlua::Error) -> String {
        match error {
            mlua::Error::CallbackError { cause, .. } | mlua::Error::BadArgument { cause, .. } => {
                category(cause)
            }
            mlua::Error::FromLuaConversionError { from, to, .. } => {
                let allowed = [
                    "nil", "boolean", "integer", "number", "string", "table", "function",
                    "userdata", "String", "Function", "Table", "Value",
                ];
                format!(
                    "NativeUI conversion {} to {}",
                    if allowed.contains(from) {
                        from
                    } else {
                        "value"
                    },
                    if allowed.contains(&to.as_str()) {
                        to
                    } else {
                        "value"
                    }
                )
            }
            mlua::Error::SyntaxError { .. } => "NativeUI Lua syntax".into(),
            mlua::Error::ExternalError(error) => {
                let message = error.to_string();
                if let Some(site) = message.strip_prefix("NativeUI interrupt budget exceeded; site ")
                    && site.bytes().all(|c| c.is_ascii_hexdigit() || c == b':')
                {
                    return message;
                }
                [
                    "NativeUI module absent",
                    "NativeUI meter unavailable",
                    "NativeUI control unavailable",
                    "NativeUI syntax translation",
                    "Invalid NativeUI module path",
                    "NativeUI interrupt budget exceeded",
                    "NativeUI time budget exceeded",
                    "NativeUI value type unsupported",
                    "invalid utf-8 sequence",
                ]
                .into_iter()
                .find(|class| message.contains(class))
                .unwrap_or("NativeUI host operation")
                .into()
            }
            mlua::Error::RuntimeError(_) => "NativeUI Lua runtime".into(),
            _ => "NativeUI Lua operation".into(),
        }
    }
    let category = error
        .downcast_ref::<mlua::Error>()
        .map(category)
        .unwrap_or_else(|| if error.to_string()=="NativeUI supplied font unavailable" {
            "NativeUI font service unavailable".into()
        } else {"NativeUI package".into()});
    format!("{phase}, {category}: {error}")
}
impl Drop for State {
    fn drop(&mut self) {
        self.canceled.store(true, Ordering::Release);
        LOCAL.with(|local| {
            if let Ok(mut local) = local.try_borrow_mut() {
                local.remove(&self.id);
            }
        });
    }
}
impl State {
    pub fn new(path: &Path, entry: &str, controls: Vec<(ir::Source, usize, ir::Widget)>) -> Self {
        let path = path.to_path_buf();
        let (done, loading) = std::sync::mpsc::sync_channel(1);
        let canceled = Arc::new(AtomicBool::new(false));
        let cancel = canceled.clone();
        let _ = std::thread::Builder::new()
            .name("native-resources".into())
            .spawn(move || {
                let result = Package::load(&path).map(Arc::new);
                if !cancel.load(Ordering::Acquire) {
                    let _ = done.send(result);
                    super::picture_worker::completed();
                }
            });
        Self {
            id: NEXT.fetch_add(1, Ordering::Relaxed),
            loading,
            canceled,
            package: None,
            failed: false,
            failure: None,
            started: false,
            entry: entry.into(),
            seed: controls,
            size: Size::new(970., 600.),
        }
    }
    pub fn bytes(&self) -> usize {
        self.package.as_ref().map_or(0, |p| p.images.bytes())
    }
    pub fn pending(&self) -> usize {
        if !self.started && !self.failed {
            1
        } else {
            self.package.as_ref().map_or(0, |p| p.images.pending())
        }
    }
    pub fn edits(&self) -> Vec<Edit> {
        LOCAL.with(|local| {
            local
                .borrow()
                .get(&self.id)
                .map_or(Vec::new(), |l| l.session.take_edits())
        })
    }
    /// Update an already-published source; no private script model or transport.
    pub fn update_view(
        &mut self,
        face: &ir::Interface,
        values: &super::ir_view::Values,
        typed: &HashMap<ir::WidgetRef, ir::Value>,
    ) {
        for (source, index, widget) in &mut self.seed {
            if *source == face.source
                && let Some(current) = face.widgets.get(*index)
            {
                widget.clone_from(current);
                if let Some(value) = typed.get(&ir::WidgetRef(*index)) {
                    widget.value = Some(value.clone());
                }
            }
        }
        LOCAL.with(|local| {
            if let Some(local) = local.borrow().get(&self.id) {
                local
                    .session
                    .update_view(face, values, typed, &HashMap::new());
            }
        });
    }
    pub fn authored(&self) -> Size {
        self.size
    }
    #[cfg(feature = "shots")]
    pub fn scan(&self) -> super::pictures::Scan {
        self.package
            .as_ref()
            .map_or(Default::default(), |p| p.images.scan())
    }
    #[cfg(feature = "shots")]
    pub fn font_success(&self) -> Option<usize> {
        self.package.as_ref().map(|p| p.fonts.len())
    }
    #[cfg(feature = "shots")]
    pub fn failures(&self) -> Vec<String> {
        self.package
            .as_ref()
            .map_or(Vec::new(), |p| p.images.failures())
    }
    #[cfg(feature = "shots")]
    pub fn diagnostic(&self) -> Option<String> {
        self.failure.as_ref().map(|raw| {
            format!(
                "{}; {}",
                raw.split_once(": ")
                    .map_or("NativeUI failure", |(category, _)| category),
                crate::scan_metrics::message(raw)
            )
        })
    }
    pub fn entry(&self) -> &str {
        &self.entry
    }
    pub fn view(
        &mut self,
        ui: &mut Ui,
        _slot: usize,
        scale: f64,
        face: &ir::Interface,
        values: &super::ir_view::Values,
        input: &super::ir_view::InputState,
    ) -> El {
        let slot = self.id as usize;
        if let Ok(result) = self.loading.try_recv() {
            match result {
                Ok(package) => self.package = Some(package),
                Err(error) => {
                    self.failed = true;
                    self.failure = Some(failure(&error, "NativeUI resource preparation"));
                }
            }
        }
        if self.failed {
            return caption("The authored native interface could not start.")
                .lines(2)
                .pad(12.);
        }
        let Some(package) = &self.package else {
            return caption("Loading the authored interface…").pad(12.);
        };
        let result = LOCAL.with(|local| -> anyhow::Result<El> {
            let mut local = local.borrow_mut();
            local.retain(|_, l| {
                l.lifetime
                    .upgrade()
                    .is_some_and(|live| !live.load(Ordering::Acquire))
            });
            if !local.contains_key(&self.id) {
                let session =
                    Session::new(package.clone(), &self.entry, std::mem::take(&mut self.seed))?;
                local.insert(
                    self.id,
                    Local {
                        lifetime: Arc::downgrade(&self.canceled),
                        session,
                        graph: None,
                        hovered: HashSet::new(),
                        drafts: HashMap::new(),
                    },
                );
            }
            self.started = true;
            let local = local.get_mut(&self.id).unwrap();
            let session = &local.session;
            session.update_view(face, values, &input.values, &input.meters);
            if let Some(graph) = &local.graph {
                events(ui, graph, session, slot, scale, &mut local.hovered)?;
            }
            let graph = session.render()?;
            let authored = authored_size(&graph);
            self.size = authored;
            let el = draw(
                ui,
                &graph,
                package,
                session,
                slot,
                scale,
                Style::default(),
                &mut local.drafts,
            )?
            .w(authored.width * scale)
            .h(authored.height * scale)
            .clip()
            .named("NativeUI performance view");
            local.graph = Some(graph);
            Ok(el)
        });
        match result {
            Ok(el) => el,
            Err(error) => {
                self.failed = true;
                self.failure = Some(failure(&error, if self.started { "NativeUI graph" } else { "NativeUI module initialization" }));
                caption("The authored native interface could not render.")
                    .lines(2)
                    .pad(12.)
            }
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
    font: Option<Font>,
}
impl Default for Style {
    fn default() -> Self {
        Self {
            ink: Color::srgb(1., 1., 1.),
            size: 12.,
            bold: false,
            disabled: false,
            hidden: false,
            font: None,
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
    let mut font_name = None;
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
            "font" | "font_family" => {
                font_name = Some(string(m, "value"));
            }
            "bold" => style.bold = boolean(&v),
            "disabled" => style.disabled |= boolean(&v),
            "hidden" => style.hidden |= boolean(&v),
            _ => {}
        }
    }
    if let Some(name) = font_name {
        style.font=Some(package.font(&name,style.bold)
            .ok_or_else(||anyhow::anyhow!("NativeUI supplied font unavailable"))?);
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
        "Text" => {
            let el = text(string(&props, "text"))
                .text_size(style.size * s)
                .fill(style.ink)
                .text_weight(if style.bold {
                    mui_text::Weight::BOLD
                } else {
                    mui_text::Weight::REGULAR
                });
            if let Some(font) = &style.font {
                el.font(font.clone())
            } else {
                el
            }
        }
        "TextInput" => {
            let id = format!("nui-{slot}-{}-text", string(node, "path"));
            let value = drafts
                .entry(id.clone())
                .or_insert_with(|| string(&props, "text"));
            if !ui.focused(id.as_str()) {
                *value = string(&props, "text");
            }
            static FALLBACK: std::sync::OnceLock<Font> = std::sync::OnceLock::new();
            let font=style.font.clone().unwrap_or_else(||FALLBACK.get_or_init(||Font::new(NOTO_SANS).unwrap()).clone());
            let nominal=style.size*s;
            // Reuse v1's bounded caption fit when the host default face is
            // approximated. Authored fonts and focused editing keep their size.
            let size=if style.font.is_none() && !ui.focused(id.as_str()) {
                ui.scene().and_then(|scene|scene.surface(&id)).map_or(nominal,|surface| {
                    let advance=mui_text::shape_run(std::slice::from_ref(&font),value,nominal,
                        &[("wght",if style.bold {700.}else{400.})]).map_or(0.,|r|r.advance);
                    if advance>0. && surface.frame.size.width>0. {
                        nominal*(surface.frame.size.width/advance).clamp(0.75,1.)
                    } else {nominal}
                })
            } else {nominal};
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
            let mut el = field
                .el
                .text_size(size)
                .font(font.clone())
                .text_weight(if style.bold { mui_text::Weight::BOLD } else { mui_text::Weight::REGULAR })
                .fill(Fill::None)
                .pad(0)
                .radius(0.);
            // The generic editor has separate paint/hit-test insets; clearing
            // layout padding alone still clips authored fields by 16 pixels.
            if let Some(edit) = &mut el.payload_mut().extras_mut().editable_text {
                edit.insets = [0., 0.];
            }
            let height=mui_text::shape_run(std::slice::from_ref(&font), "M", size,
                &[("wght", if style.bold { 700. } else { 400. })])
                .map_or(size*1.25,|run|run.line_height);
            el.h(height)
        }
        "Rectangle" => block(Len::Auto, Len::Auto)
            .fill(color(&val(&props, "color")).unwrap_or(style.ink))
            .radius(number(&props, "corner_radius").unwrap_or(0.) * s),
        "Image" => {
            let file = string(&props, "file").replace('\\', "/").to_lowercase();
            let image = package.images.get(&file);
            let width = image.as_ref().map_or(1, |i| i.width);
            let height = image.as_ref().map_or(1, |i| i.height);
            let resizable = boolean(&val(&props, "resizable"));
            let el = block(
                if resizable {
                    Len::Auto
                } else {
                    Len::Px(f64::from(width) * s)
                },
                if resizable {
                    Len::Auto
                } else {
                    Len::Px(f64::from(height) * s)
                },
            )
            .fill(image.map_or(Fill::None, |i| Fill::Image(i, Fit::Fill)));
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires locally owned NativeUI library; graph and bindings stay in RAM"]
    fn native_edit_selector_bindings_use_the_published_source() {
        let path=std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
        let mut source=sampler_kontakt::read(&path).unwrap().instrument;
        source.zones.clear(); source.source_indices.zones.clear(); source.assets.clear();
        let loaded=sampler_kontakt::prepare(source,vec![],&sampler_kontakt::Options {
            library:Some(path.clone()),..Default::default()
        }).unwrap();
        let entry=loaded.interfaces.iter().find_map(|f|f.native_ui.as_ref()).unwrap().entry.clone();
        let controls:Vec<_>=loaded.interfaces.iter().flat_map(|f|f.widgets.iter().enumerate()
            .map(move |(n,w)|(f.source,n,w.clone()))).collect();
        let package=Arc::new(Package::load(&path).unwrap());
        let session=Session::new(package,&entry,controls.clone()).unwrap();
        let reads=Arc::new(std::sync::Mutex::new(Vec::<(String,usize,String)>::new()));
        let trace=reads.clone();
        session.lua().globals().set("__audit_parameter",session.lua().create_function(
            move |_,(name,binding,path):(String,i64,String)| {
                if binding>=0 && (name.starts_with("Edit__Synth__Src__") || name.starts_with("Edit__Synth__Shp__")) {
                    let mut trace=trace.lock().unwrap();
                    if trace.len()<16384 {trace.push((name,binding as usize,path));}
                }
                Ok(())
            }
        ).unwrap()).unwrap();
        fn tap(node:&Table,target:&str)->Option<Function> {
            let mut contains=string(node,"kind")=="Text" &&
                node.get::<Table>("props").ok().is_some_and(|p|string(&p,"text").eq_ignore_ascii_case(target));
            for child in tables(node,"children").unwrap() {
                if let Some(callback)=tap(&child,target) {return Some(callback);}
                contains|=has_text(&child,target);
            }
            if contains {for modifier in tables(node,"modifiers").unwrap() {
                if string(&modifier,"name")=="on_tap_gesture" &&
                    let Ok(value)=modifier.get::<Table>("value") &&
                    let Ok(callback)=value.get::<Function>("complete").or_else(|_|value.get("start")) {return Some(callback);}
                if matches!(string(&modifier,"name").as_str(),"background"|"overlay") &&
                    let Ok(child)=modifier.get::<Table>("value") &&
                    let Some(callback)=tap(&child,target) {return Some(callback);}
            }}
            None
        }
        fn has_text(node:&Table,target:&str)->bool {
            (string(node,"kind")=="Text" && node.get::<Table>("props").ok()
                .is_some_and(|p|string(&p,"text").eq_ignore_ascii_case(target))) ||
                tables(node,"children").unwrap().iter().any(|c|has_text(c,target))
        }
        let graph=session.render().unwrap_or_else(|e|panic!("{}",failure(&e,"graph").split_once(": ").unwrap().0));
        let callback=tap(&graph,"EDIT").expect("authored Edit tab has a tap callback");
        session.call(callback,session.event(0.,0.,0.,0.,0.,0.,false,false,false,false).unwrap()).unwrap();
        reads.lock().unwrap().clear();
        let graph=session.render().unwrap_or_else(|e|panic!("{}",failure(&e,"graph").split_once(": ").unwrap().0));
        fn node_kinds(node:&Table,out:&mut HashMap<String,String>) {
            out.insert(string(node,"path"),string(node,"kind"));
            for child in tables(node,"children").unwrap() {node_kinds(&child,out);}
            for modifier in tables(node,"modifiers").unwrap() {
                if matches!(string(&modifier,"name").as_str(),"background"|"overlay") &&
                    let Ok(child)=modifier.get::<Table>("value") {node_kinds(&child,out);}
            }
        }
        let mut kinds=HashMap::new();node_kinds(&graph,&mut kinds);
        let mut seen=std::collections::BTreeSet::new();
        for (name,binding,path) in reads.lock().unwrap().iter() {
            if !(name.starts_with("Edit__Synth__Src__") || name.starts_with("Edit__Synth__Shp__")) {continue;}
            let kind=kinds.get(path).map(String::as_str).unwrap_or("Component");
            if !seen.insert((name.clone(),kind.to_owned(),*binding)) {continue;}
            let (source,_,widget)=controls.get(*binding).expect("binding addresses a published control");
            println!("NATIVE_BINDING node={kind} parameter={name} source={source:?} ui_id={:?} binding={:?}",widget.source_id,widget.binding);
        }
        assert!(!seen.is_empty(),"Edit graph reads published selector parameters");
    }

    #[test]
    #[ignore = "requires locally owned NativeUI library; text stays in RAM"]
    fn native_saved_text_reaches_authored_field_without_host_insets() {
        let path = std::path::PathBuf::from(std::env::var_os("KONTRA_AUDIT_WIDGET_PATCH").unwrap());
        let mut source = sampler_kontakt::read(&path).unwrap().instrument;
        source.retain_zones(|_|false);
        source.assets.clear();
        let loaded = sampler_kontakt::prepare(source, vec![], &sampler_kontakt::Options {
            library: Some(path.clone()), ..Default::default()
        }).unwrap();
        let entry = loaded.interfaces.iter().find_map(|f|f.native_ui.as_ref()).unwrap().entry.clone();
        let controls: Vec<_> = loaded.interfaces.iter().flat_map(|f|f.widgets.iter().enumerate()
            .map(move |(n,w)|(f.source,n,w.clone()))).collect();
        let saved: Vec<_> = controls.iter().filter(|(_,_,w)|w.name.starts_with("@Footer__Macro__Name__"))
            .map(|(_,_,w)|match &w.value {Some(ir::Value::Text(s))=>s.clone(),_=>panic!("published text missing")}).collect();
        assert_eq!(saved.len(), 6);
        let package = Arc::new(Package::load(&path).unwrap());
        let session = Session::new(package.clone(), &entry, controls).unwrap();
        let graph = session.render().unwrap();
        fn font_usage(node:&Table,package:&Package,counts:&mut [usize;3]) {
            for modifier in tables(node,"modifiers").unwrap() {
                let kind=string(&modifier,"name");
                if matches!(kind.as_str(),"font"|"font_family") {
                    counts[0]+=1;
                    let name=string(&modifier,"value");
                    counts[1]+=usize::from(!name.is_empty());
                    counts[2]+=usize::from(package.font(&name,false).is_some());
                }
                if matches!(kind.as_str(),"background"|"overlay") {
                    if let Ok(child)=modifier.get::<Table>("value") {font_usage(&child,package,counts);}
                }
            }
            for child in tables(node,"children").unwrap() {font_usage(&child,package,counts);}
        }
        let mut fonts=[0;3];font_usage(&graph,&package,&mut fonts);
        println!("NATIVE_FONT declarations={} string_names={} supplied_matches={}",fonts[0],fonts[1],fonts[2]);
        fn fields(node:&Table,saved:&[String],out:&mut Vec<(String,usize,usize)>) {
            let kind = string(node,"kind");
            if matches!(kind.as_str(),"Text"|"TextInput") {
                let props=node.get::<Table>("props").unwrap();
                let text = string(&props,"text");
                if let Some(n)=saved.iter().position(|s|s==&text) {
                    println!("NATIVE_TEXT_PROPS slot={n} font_size={:?} size={:?} family_declared={} font_table={} style_table={}",number(&props,"font_size"),number(&props,"size"),!string(&props,"font_family").is_empty(),matches!(val(&props,"font"),Value::Table(_)),matches!(val(&props,"style"),Value::Table(_)));
                    out.push((string(node,"path"),n,text.chars().count()));
                }
            }
            for child in tables(node,"children").unwrap() {fields(&child,saved,out);}
            for modifier in tables(node,"modifiers").unwrap() {
                if matches!(string(&modifier,"name").as_str(),"background"|"overlay") {
                    if let Ok(child)=modifier.get::<Table>("value") {fields(&child,saved,out);}
                }
            }
        }
        let mut fields_found=Vec::new();
        fields(&graph,&saved,&mut fields_found);
        assert!(fields_found.len()>=6,"six complete saved strings reach native primitives");
        let mut ui=super::super::theme::ui();
        let mut drafts=HashMap::new();
        for _ in 0..4 {
            let el=draw(&mut ui,&graph,&package,&session,0,1.,Style::default(),&mut drafts).unwrap();
            ui.frame(el,Some(authored_size(&graph)),Input::default(),1./60.).unwrap();
        }
        let scene=ui.scene().unwrap();
        let mut matched=0;
        for (path,n,len) in fields_found {
            if let Some(surface)=scene.surface(&format!("nui-0-{path}-text")) {
                let geometry=surface.text_geometry.as_ref().unwrap();
                let advance=geometry.lines[0].carets.x(geometry.text.len());
                println!("NATIVE_TEXT slot={n} chars={len} equals_saved={} frame_width={} viewport_width={} advance={} insets={:?}",
                    geometry.text.as_ref()==saved[n],surface.frame.size.width,geometry.viewport.width,advance,geometry.state.insets);
                assert!(geometry.text.as_ref()==saved[n],"full saved text reaches final field geometry");
                assert_eq!(geometry.state.insets,[0.,0.],"authored text fields must not retain generic editor padding");
                assert!(advance<=geometry.viewport.width+0.01,"fallback metrics fit the complete saved caption");
                matched+=1;
            }
        }
        assert_eq!(matched,6);
        let modifiers=graph.get::<Table>("modifiers").unwrap();
        let missing=session.lua().create_table().unwrap();
        missing.set("name","font_family").unwrap();
        missing.set("value","unavailable-test-font").unwrap();
        modifiers.push(missing).unwrap();
        let error=draw(&mut ui,&graph,&package,&session,0,1.,Style::default(),&mut drafts).err().unwrap();
        assert!(failure(&error,"graph").starts_with("graph, NativeUI font service unavailable:"));
    }
}
